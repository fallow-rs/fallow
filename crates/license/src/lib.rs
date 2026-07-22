//! Offline Ed25519-signed license JWT verification for the fallow CLI.
//!
//! This crate is the public-binary side of fallow's paid-feature gating. It
//! does NOT perform any network I/O; the license file is loaded from disk or
//! environment, the signature is verified against a public key compiled in by
//! the embedding binary, and the result is exposed as a [`LicenseStatus`].
//!
//! # Storage precedence
//!
//! License material is sourced in this order (first match wins):
//!
//! 1. `$FALLOW_LICENSE` environment variable (full JWT string).
//! 2. `$FALLOW_LICENSE_PATH` environment variable (path to a file containing the JWT).
//! 3. `~/.fallow/license.jwt` (default path under the user's home directory).
//!
//! # Algorithm pinning
//!
//! Only Ed25519 (`EdDSA`) is accepted. The JWT header's `alg` claim is verified
//! to equal `"EdDSA"` *after* base64 decoding; we never trust the header to pick
//! the algorithm.
//!
//! # Grace ladder
//!
//! Matches Docker Desktop / JetBrains conventions. See [`grace_state`].

#![forbid(unsafe_code)]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

/// Default cap on the grace window before hard-fail in the public CLI.
///
/// The enterprise binary (`--features enterprise-license`) lifts this cap.
pub const DEFAULT_HARD_FAIL_DAYS: u64 = 30;

/// Days post-expiry after which the public output gains a visible watermark.
pub const WATERMARK_DAYS: u64 = 7;

/// Default tolerance (in seconds) for `iat` clock skew: 24h.
///
/// Matches the leeway defaults used by `jsonwebtoken` (Node),
/// `pyjwt`, and `jjwt`. A JWT whose `iat` is more than this many seconds in
/// the future relative to the local clock is rejected as
/// [`LicenseError::ClockSkew`]. Override via
/// `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS` (consumed by
/// [`skew_tolerance_seconds_from_env`]).
pub const DEFAULT_SKEW_TOLERANCE_SECONDS: i64 = 86_400;

/// Env var name for overriding [`DEFAULT_SKEW_TOLERANCE_SECONDS`].
pub const SKEW_TOLERANCE_ENV: &str = "FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS";

/// JWT claims emitted by `api.fallow.cloud` for fallow CLI licenses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseClaims {
    /// Issuer (typically `"https://api.fallow.cloud"`).
    pub iss: String,
    /// Subject — opaque org identifier.
    pub sub: String,
    /// Tenant identifier.
    pub tid: String,
    /// Number of seats licensed.
    pub seats: u32,
    /// Tier string emitted by fallow-cloud: `pro`, `enterprise`, `trial`, `founding`.
    /// (`team` is the legacy name for `pro`; the server now emits `pro`.) The
    /// value is informational only: capability gating is on `features`, never on
    /// this string, so any tier value is tolerated.
    pub tier: String,
    /// Feature flags. Modeled as strings on the wire for forward-compat;
    /// callers convert to [`Feature`] for matching.
    pub features: Vec<String>,
    /// Issued-at, seconds since UNIX epoch.
    pub iat: i64,
    /// Expiration, seconds since UNIX epoch.
    pub exp: i64,
    /// Unique JWT ID (used for refresh + revocation).
    pub jti: String,
    /// Suggested refresh timestamp, seconds since UNIX epoch. Backend emits
    /// this at `iat + 15 days` so CI runs can proactively refresh before the
    /// hard-fail window. `None` when the backend did not include the claim
    /// (older license payloads or third-party issuers).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_after: Option<i64>,
}

/// Feature flag enum aligned with the protocol's `Feature` strings.
///
/// Wire format stays a string array; new variants are additive in minor protocol
/// bumps and unrecognized strings round-trip through [`Feature::Other`].
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Feature {
    /// Paid local runtime coverage analyzer (CLI + sidecar).
    RuntimeCoverage,
    /// Cloud portfolio dashboard. Currently inert: granted in JWTs but not
    /// yet consumed by any CLI command.
    PortfolioDashboard,
    /// Cloud MCP tools. Currently inert: granted in JWTs but not yet
    /// consumed by any CLI command.
    McpCloudTools,
    /// Cross-repo aggregation. Currently inert: granted in JWTs but not yet
    /// consumed by any CLI command.
    CrossRepoAggregation,
    /// Forward-compat sentinel for unrecognized feature strings.
    Other(String),
}

impl Feature {
    /// Parse a wire string into a [`Feature`]. Unrecognized strings round-trip
    /// through [`Feature::Other`] so older CLIs do not error on newer license
    /// payloads.
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "runtime_coverage" => Self::RuntimeCoverage,
            "portfolio_dashboard" => Self::PortfolioDashboard,
            "mcp_cloud_tools" => Self::McpCloudTools,
            "cross_repo_aggregation" => Self::CrossRepoAggregation,
            other => Self::Other(other.to_owned()),
        }
    }
}

impl LicenseClaims {
    /// True if the license's `features` claim contains the requested feature.
    #[must_use]
    pub fn has_feature(&self, feature: &Feature) -> bool {
        self.features.iter().any(|s| Feature::parse(s) == *feature)
    }
}

/// Outcome of [`load_and_verify`].
#[derive(Debug, Clone)]
pub enum LicenseStatus {
    /// License is valid and not yet expired.
    Valid {
        claims: LicenseClaims,
        days_until_expiry: i64,
    },
    /// License is in the warning window (0..[`WATERMARK_DAYS`] days post-expiry).
    /// Analysis runs normally; human output prints a refresh hint.
    ExpiredWarning {
        claims: LicenseClaims,
        days_since_expiry: u64,
    },
    /// License is in the watermark window
    /// ([`WATERMARK_DAYS`]..hard_fail_days post-expiry). Analysis runs but
    /// every human-facing surface gains a visible "license expired" watermark.
    ExpiredWatermark {
        claims: LicenseClaims,
        days_since_expiry: u64,
    },
    /// License is past the hard-fail cap. Analysis must NOT run.
    HardFail {
        claims: LicenseClaims,
        days_since_expiry: u64,
    },
    /// No license material was found at any of the precedence locations.
    Missing,
}

impl LicenseStatus {
    /// True if the holder is allowed to use paid features (any non-hard-fail
    /// state with the requested feature in the claims).
    #[must_use]
    pub fn permits(&self, feature: &Feature) -> bool {
        match self {
            Self::Valid { claims, .. }
            | Self::ExpiredWarning { claims, .. }
            | Self::ExpiredWatermark { claims, .. } => claims.has_feature(feature),
            Self::HardFail { .. } | Self::Missing => false,
        }
    }

    /// True if a watermark string should be appended to user-facing output.
    #[must_use]
    pub const fn show_watermark(&self) -> bool {
        matches!(self, Self::ExpiredWatermark { .. })
    }
}

/// Errors returned by [`load_and_verify`] when the license material is present
/// but malformed (vs simply missing, which is reported via [`LicenseStatus::Missing`]).
#[derive(Debug)]
pub enum LicenseError {
    /// I/O error reading the license file.
    Io(std::io::Error),
    /// JWT structure was not three base64url-encoded segments.
    MalformedJwt(String),
    /// Header could not be parsed as JSON or had wrong `alg`.
    BadHeader(String),
    /// Payload could not be parsed as [`LicenseClaims`].
    BadPayload(String),
    /// Signature verification failed.
    BadSignature,
    /// JWT length looks truncated (typical valid range 700-1500 chars).
    Truncated { actual: usize },
    /// The license JWT's `iat` claim is more than the configured tolerance in
    /// the future relative to the local clock. Mathematically equivalent to
    /// "the local clock is more than the tolerance behind the license issue
    /// time"; the two interpretations are the same condition.
    ///
    /// Tolerance is applied only to `iat`, not to `exp`. The existing grace
    /// ladder (7 / 30 / hard-fail) absorbs sub-day `exp` skew. This is a
    /// deliberate asymmetry; revisit if a real incident shows otherwise.
    ClockSkew {
        /// JWT `iat` claim (unix seconds).
        iat_seconds: i64,
        /// Local clock at verification time (unix seconds).
        now_seconds: i64,
        /// Tolerance window applied (seconds).
        tolerance_seconds: i64,
    },
}

impl std::fmt::Display for LicenseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "license I/O error: {err}"),
            Self::MalformedJwt(msg) => write!(f, "malformed JWT: {msg}"),
            Self::BadHeader(msg) => write!(f, "bad JWT header: {msg}"),
            Self::BadPayload(msg) => write!(f, "bad JWT payload: {msg}"),
            Self::BadSignature => write!(f, "JWT signature verification failed"),
            Self::Truncated { actual } => write!(
                f,
                "the token looks truncated (got {actual} chars; expected 700+). Did you copy the whole thing? Try: fallow license activate --from-file license.jwt"
            ),
            Self::ClockSkew {
                iat_seconds,
                now_seconds,
                tolerance_seconds,
            } => {
                let delta = iat_seconds.saturating_sub(*now_seconds).unsigned_abs();
                let tolerance = u64::try_from(*tolerance_seconds).unwrap_or(0);
                write!(
                    f,
                    "license appears to be issued {duration} in the future (allowed skew {tolerance_human}). The system clock and the license issue time differ significantly; this commonly happens in CI containers without NTP, on machines with a dead BIOS battery, or when a clock has drifted. After confirming your clock is correct, set {env}=<seconds> to override the default 24h window.",
                    duration = format_duration_seconds(delta),
                    tolerance_human = format_duration_seconds(tolerance),
                    env = SKEW_TOLERANCE_ENV,
                )
            }
        }
    }
}

impl std::error::Error for LicenseError {}

impl From<std::io::Error> for LicenseError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Verify a raw JWT string against the supplied public key and (optionally)
/// the wall clock. The `now` parameter is the unix-seconds reference used to
/// classify expiry; pass [`current_unix_seconds`] in production.
///
/// Delegates to [`verify_jwt_with_skew`] with [`DEFAULT_SKEW_TOLERANCE_SECONDS`]
/// so existing callers retain the same signature; new code that needs to
/// honor the `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS` env var should call
/// [`verify_jwt_with_skew`] directly with [`skew_tolerance_seconds_from_env`].
pub fn verify_jwt(
    raw_jwt: &str,
    public_key: &VerifyingKey,
    now: i64,
    hard_fail_days: u64,
) -> Result<LicenseStatus, LicenseError> {
    verify_jwt_with_skew(
        raw_jwt,
        public_key,
        now,
        hard_fail_days,
        DEFAULT_SKEW_TOLERANCE_SECONDS,
    )
}

/// Verify a raw JWT string with an explicit clock-skew tolerance.
///
/// Rejects JWTs whose `iat` is more than `skew_tolerance_seconds` in the
/// future relative to `now`. The same condition catches both forward-signed
/// JWTs and systems whose clocks are behind reality (since
/// `now < iat - tolerance` is equivalent to `iat > now + tolerance`).
/// Tolerance is applied only to `iat`; `exp` continues to flow through the
/// grace ladder unchanged.
pub fn verify_jwt_with_skew(
    raw_jwt: &str,
    public_key: &VerifyingKey,
    now: i64,
    hard_fail_days: u64,
    skew_tolerance_seconds: i64,
) -> Result<LicenseStatus, LicenseError> {
    let trimmed = normalize_jwt(raw_jwt);

    if trimmed.len() < 200 {
        return Err(LicenseError::Truncated {
            actual: trimmed.len(),
        });
    }

    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() != 3 {
        return Err(LicenseError::MalformedJwt(format!(
            "expected 3 segments, got {}",
            parts.len()
        )));
    }
    let (header_b64, payload_b64, signature_b64) = (parts[0], parts[1], parts[2]);

    let header_bytes = URL_SAFE_NO_PAD
        .decode(header_b64)
        .map_err(|err| LicenseError::BadHeader(format!("base64 decode: {err}")))?;
    let header: serde_json::Value = serde_json::from_slice(&header_bytes)
        .map_err(|err| LicenseError::BadHeader(format!("json parse: {err}")))?;
    let alg = header
        .get("alg")
        .and_then(|v| v.as_str())
        .ok_or_else(|| LicenseError::BadHeader("missing alg claim".to_owned()))?;
    if alg != "EdDSA" {
        return Err(LicenseError::BadHeader(format!(
            "expected alg=EdDSA, got alg={alg}"
        )));
    }

    let signature_bytes = URL_SAFE_NO_PAD
        .decode(signature_b64)
        .map_err(|_| LicenseError::BadSignature)?;
    let signature_array: [u8; 64] = signature_bytes
        .as_slice()
        .try_into()
        .map_err(|_| LicenseError::BadSignature)?;
    let signature = Signature::from_bytes(&signature_array);
    let signing_input = format!("{header_b64}.{payload_b64}");
    public_key
        .verify_strict(signing_input.as_bytes(), &signature)
        .map_err(|_| LicenseError::BadSignature)?;

    let payload_bytes = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|err| LicenseError::BadPayload(format!("base64 decode: {err}")))?;
    let claims: LicenseClaims = serde_json::from_slice(&payload_bytes)
        .map_err(|err| LicenseError::BadPayload(format!("json parse: {err}")))?;

    let earliest_iat = now.saturating_add(skew_tolerance_seconds);
    if claims.iat > earliest_iat {
        return Err(LicenseError::ClockSkew {
            iat_seconds: claims.iat,
            now_seconds: now,
            tolerance_seconds: skew_tolerance_seconds,
        });
    }

    Ok(grace_state(claims, now, hard_fail_days))
}

/// Map a verified [`LicenseClaims`] to a [`LicenseStatus`] using the 7/cap/hard-fail
/// ladder.
#[must_use]
pub fn grace_state(claims: LicenseClaims, now: i64, hard_fail_days: u64) -> LicenseStatus {
    let delta_seconds = i64::from(claims.exp != 0) * (claims.exp - now);
    if delta_seconds >= 0 {
        return LicenseStatus::Valid {
            days_until_expiry: delta_seconds / SECONDS_PER_DAY,
            claims,
        };
    }
    let days_since_expiry = (delta_seconds.unsigned_abs()).div_ceil(SECONDS_PER_DAY.unsigned_abs());
    if days_since_expiry > hard_fail_days {
        LicenseStatus::HardFail {
            claims,
            days_since_expiry,
        }
    } else if days_since_expiry > WATERMARK_DAYS {
        LicenseStatus::ExpiredWatermark {
            claims,
            days_since_expiry,
        }
    } else {
        LicenseStatus::ExpiredWarning {
            claims,
            days_since_expiry,
        }
    }
}

/// Discover and load a license JWT according to the storage precedence rules,
/// then verify it and apply the grace ladder.
///
/// Returns `Ok(LicenseStatus::Missing)` when no source provides material; an
/// `Err(LicenseError)` only when material was present but malformed.
pub fn load_and_verify(
    public_key: &VerifyingKey,
    hard_fail_days: u64,
) -> Result<LicenseStatus, LicenseError> {
    let now = current_unix_seconds();
    let skew = skew_tolerance_seconds_from_env();
    match load_raw_jwt()? {
        Some(jwt) => verify_jwt_with_skew(&jwt, public_key, now, hard_fail_days, skew),
        None => Ok(LicenseStatus::Missing),
    }
}

/// Resolve the JWT source according to [storage precedence](crate#storage-precedence).
///
/// Returns `Ok(None)` when no source provides material.
pub fn load_raw_jwt() -> Result<Option<String>, LicenseError> {
    if let Ok(jwt) = std::env::var("FALLOW_LICENSE") {
        let trimmed = normalize_jwt(&jwt);
        if !trimmed.is_empty() {
            return Ok(Some(trimmed));
        }
    }
    if let Some(path) = resolve_license_path_env(std::env::var("FALLOW_LICENSE_PATH").ok()) {
        return Ok(Some(read_jwt_file(&path)?));
    }
    let default = default_license_path();
    if default.exists() {
        return Ok(Some(read_jwt_file(&default)?));
    }
    Ok(None)
}

/// Normalize a raw `$FALLOW_LICENSE_PATH` env value. Returns `None` when the
/// var is unset, empty, or whitespace-only so the caller falls through to
/// default-path discovery; otherwise returns the trimmed path. Without this,
/// shells that export `FALLOW_LICENSE_PATH=""` (empty-string) produced a
/// cryptic `license I/O error: No such file or directory` on `health
/// --runtime-coverage` because `read_jwt_file(Path::new(""))` fails at the
/// fs layer.
fn resolve_license_path_env(raw: Option<String>) -> Option<PathBuf> {
    let raw = raw?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn read_jwt_file(path: &Path) -> Result<String, LicenseError> {
    let raw = std::fs::read_to_string(path)?;
    Ok(normalize_jwt(&raw))
}

/// Resolve the user's home directory in a cross-platform way.
///
/// Checks `$HOME` first (standard on Unix and set by Git Bash / MSYS /
/// Cygwin on Windows), then `%USERPROFILE%` (native Windows). Returns
/// `None` only when neither resolves to a non-empty string, which in
/// practice means a bare container with no home set — callers decide
/// whether to fall back to cwd or error.
#[must_use]
pub fn user_home_dir() -> Option<PathBuf> {
    user_home_from_env(|key| std::env::var(key).ok())
}

fn user_home_from_env(getenv: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    for key in ["HOME", "USERPROFILE"] {
        if let Some(value) = getenv(key)
            && !value.is_empty()
        {
            return Some(PathBuf::from(value));
        }
    }
    None
}

/// Compute the canonical default license path (`~/.fallow/license.jwt`).
///
/// On Unix this reads `$HOME`; on Windows it falls back to `%USERPROFILE%`
/// when `$HOME` is not set (native cmd / PowerShell). Falls back to
/// `./.fallow/license.jwt` if neither resolves — exotic containers and
/// CI sandboxes being the usual suspects.
#[must_use]
pub fn default_license_path() -> PathBuf {
    user_home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".fallow")
        .join("license.jwt")
}

/// Strip whitespace and embedded line breaks from a pasted JWT.
///
/// Shells routinely fold long tokens onto multiple lines, especially via
/// PowerShell or zsh's bracketed-paste. This is the single normalization
/// hook used by every input path (env var, file, CLI arg, stdin).
#[must_use]
pub fn normalize_jwt(raw: &str) -> String {
    raw.chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
}

/// Wrapper around `SystemTime::now()` returning unix seconds.
///
/// Returns `0` if the system clock is before the unix epoch (impossible in
/// practice — included to avoid `unwrap`).
#[must_use]
pub fn current_unix_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

const SECONDS_PER_DAY: i64 = 86_400;

/// Resolve the clock-skew tolerance (in seconds) from
/// `FALLOW_LICENSE_SKEW_TOLERANCE_SECONDS`, falling back to
/// [`DEFAULT_SKEW_TOLERANCE_SECONDS`] when the variable is unset, empty,
/// whitespace-only, or unparsable.
///
/// Parsing is lenient by design: a typo in a CI runner's env block must not
/// fail license verification. The value is parsed as `u64` and capped at
/// `i64::MAX`, so any positive integer is accepted.
#[must_use]
pub fn skew_tolerance_seconds_from_env() -> i64 {
    skew_tolerance_seconds_from(|key| std::env::var(key).ok())
}

fn skew_tolerance_seconds_from(getenv: impl Fn(&str) -> Option<String>) -> i64 {
    let Some(raw) = getenv(SKEW_TOLERANCE_ENV) else {
        return DEFAULT_SKEW_TOLERANCE_SECONDS;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return DEFAULT_SKEW_TOLERANCE_SECONDS;
    }
    match trimmed.parse::<u64>() {
        Ok(value) => i64::try_from(value).unwrap_or(i64::MAX),
        Err(_) => DEFAULT_SKEW_TOLERANCE_SECONDS,
    }
}

/// Render a duration in seconds as a human-friendly string. Used by
/// [`LicenseError::ClockSkew`]'s [`Display`] impl so users see "2 days"
/// instead of "172800 seconds".
///
/// Integer floor at each tier; no fractional units. Tiers:
/// `< 60s` -> "N seconds", `< 3600s` -> "M minutes", `< 86_400s` ->
/// "H hours [M minutes]", `>= 86_400s` -> "D days [H hours]".
///
/// [`Display`]: std::fmt::Display
fn format_duration_seconds(seconds: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;

    fn unit(value: u64, singular: &str) -> String {
        if value == 1 {
            format!("1 {singular}")
        } else {
            format!("{value} {singular}s")
        }
    }

    if seconds < MINUTE {
        return unit(seconds, "second");
    }
    if seconds < HOUR {
        return unit(seconds / MINUTE, "minute");
    }
    if seconds < DAY {
        let hours = seconds / HOUR;
        let minutes = (seconds % HOUR) / MINUTE;
        if minutes == 0 {
            return unit(hours, "hour");
        }
        return format!("{} {}", unit(hours, "hour"), unit(minutes, "minute"));
    }
    let days = seconds / DAY;
    let hours = (seconds % DAY) / HOUR;
    if hours == 0 {
        return unit(days, "day");
    }
    format!("{} {}", unit(days, "day"), unit(hours, "hour"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use ed25519_dalek::{Signer, SigningKey};

    fn fixed_keypair() -> (SigningKey, VerifyingKey) {
        let signing = SigningKey::from_bytes(&[42u8; 32]);
        let verifying = signing.verifying_key();
        (signing, verifying)
    }

    fn sign_jwt(signing: &SigningKey, claims: &LicenseClaims) -> String {
        let header = serde_json::json!({"alg": "EdDSA", "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(claims).unwrap());
        let signing_input = format!("{header_b64}.{payload_b64}");
        let signature = signing.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        format!("{header_b64}.{payload_b64}.{sig_b64}")
    }

    fn make_claims(exp: i64) -> LicenseClaims {
        LicenseClaims {
            iss: "https://api.fallow.cloud".into(),
            sub: "org_test".into(),
            tid: "tenant_test".into(),
            seats: 5,
            tier: "pro".into(),
            features: vec!["runtime_coverage".into()],
            iat: 1_700_000_000,
            exp,
            jti: "jti_test".into(),
            refresh_after: Some(1_700_000_000 + 15 * SECONDS_PER_DAY),
        }
    }

    #[test]
    fn valid_jwt_passes_verification() {
        let (signing, verifying) = fixed_keypair();
        let claims = make_claims(2_000_000_000);
        let jwt = sign_jwt(&signing, &claims);
        let status = verify_jwt(&jwt, &verifying, 1_900_000_000, DEFAULT_HARD_FAIL_DAYS).unwrap();
        assert!(matches!(status, LicenseStatus::Valid { .. }));
        assert!(status.permits(&Feature::RuntimeCoverage));
        assert!(!status.permits(&Feature::PortfolioDashboard));
    }

    #[test]
    fn tampered_payload_fails_signature() {
        let (signing, verifying) = fixed_keypair();
        let claims = make_claims(2_000_000_000);
        let mut jwt = sign_jwt(&signing, &claims);
        let mid = jwt.find('.').unwrap() + 5;
        let bad: String = jwt
            .chars()
            .enumerate()
            .map(|(i, c)| if i == mid { 'X' } else { c })
            .collect();
        jwt = bad;
        let err = verify_jwt(&jwt, &verifying, 1_900_000_000, DEFAULT_HARD_FAIL_DAYS).unwrap_err();
        assert!(matches!(
            err,
            LicenseError::BadSignature | LicenseError::BadPayload(_)
        ));
    }

    #[test]
    fn rs256_header_rejected() {
        let (signing, verifying) = fixed_keypair();
        let header = serde_json::json!({"alg": "RS256", "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let claims = make_claims(2_000_000_000);
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let signing_input = format!("{header_b64}.{payload_b64}");
        let signature = signing.sign(signing_input.as_bytes());
        let sig_b64 = URL_SAFE_NO_PAD.encode(signature.to_bytes());
        let jwt = format!("{header_b64}.{payload_b64}.{sig_b64}");
        let err = verify_jwt(&jwt, &verifying, 1_900_000_000, DEFAULT_HARD_FAIL_DAYS).unwrap_err();
        assert!(matches!(err, LicenseError::BadHeader(_)));
    }

    #[test]
    fn alg_none_rejected() {
        let (_, verifying) = fixed_keypair();
        let header = serde_json::json!({"alg": "none", "typ": "JWT"});
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
        let claims = make_claims(2_000_000_000);
        let payload_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
        let jwt = format!("{header_b64}.{payload_b64}.");
        let err = verify_jwt(&jwt, &verifying, 1_900_000_000, DEFAULT_HARD_FAIL_DAYS).unwrap_err();
        assert!(matches!(err, LicenseError::BadHeader(_)));
    }

    #[test]
    fn truncated_token_returns_specific_error() {
        let (_, verifying) = fixed_keypair();
        let err = verify_jwt("eyJh.short", &verifying, 0, DEFAULT_HARD_FAIL_DAYS).unwrap_err();
        assert!(matches!(err, LicenseError::Truncated { .. }));
    }

    #[test]
    fn whitespace_in_jwt_normalized() {
        let raw = "eyJ\n  abcd\r\nef.gh\nij.kl  mn";
        assert_eq!(normalize_jwt(raw), "eyJabcdef.ghij.klmn");
    }

    #[test]
    fn normalize_jwt_empty_string_stays_empty() {
        assert!(normalize_jwt("").is_empty());
    }

    #[test]
    fn normalize_jwt_whitespace_only_becomes_empty() {
        assert!(normalize_jwt("   ").is_empty());
        assert!(normalize_jwt("\t\n\r ").is_empty());
    }

    #[test]
    fn grace_ladder_classifies_correctly() {
        let claims = make_claims(1_000_000_000);
        assert!(matches!(
            grace_state(claims.clone(), 1_000_000_000, 30),
            LicenseStatus::Valid { .. }
        ));
        assert!(matches!(
            grace_state(claims.clone(), 1_000_000_000 + 3 * SECONDS_PER_DAY, 30),
            LicenseStatus::ExpiredWarning { .. }
        ));
        assert!(matches!(
            grace_state(claims.clone(), 1_000_000_000 + 15 * SECONDS_PER_DAY, 30),
            LicenseStatus::ExpiredWatermark { .. }
        ));
        assert!(matches!(
            grace_state(claims, 1_000_000_000 + 35 * SECONDS_PER_DAY, 30),
            LicenseStatus::HardFail { .. }
        ));
    }

    #[test]
    fn watermark_status_only_in_watermark_window() {
        let claims = make_claims(1_000_000_000);
        let valid = grace_state(claims.clone(), 1_000_000_000 - 100, 30);
        let warn = grace_state(claims.clone(), 1_000_000_000 + 3 * SECONDS_PER_DAY, 30);
        let watermark = grace_state(claims.clone(), 1_000_000_000 + 15 * SECONDS_PER_DAY, 30);
        let hard = grace_state(claims, 1_000_000_000 + 60 * SECONDS_PER_DAY, 30);

        assert!(!valid.show_watermark());
        assert!(!warn.show_watermark());
        assert!(watermark.show_watermark());
        assert!(!hard.show_watermark());
    }

    #[test]
    fn permits_short_circuits_on_hard_fail() {
        let claims = make_claims(1_000_000_000);
        let hard = grace_state(claims, 1_000_000_000 + 60 * SECONDS_PER_DAY, 30);
        assert!(!hard.permits(&Feature::RuntimeCoverage));
    }

    #[test]
    fn unknown_feature_round_trips_through_other() {
        let parsed = Feature::parse("future_feature");
        assert!(matches!(parsed, Feature::Other(ref s) if s == "future_feature"));
    }

    #[test]
    fn refresh_after_parses_when_present_and_defaults_to_none() {
        let with_refresh = serde_json::json!({
            "iss": "https://api.fallow.cloud",
            "sub": "org_test",
            "tid": "tenant_test",
            "seats": 5,
            "tier": "pro",
            "features": ["runtime_coverage"],
            "iat": 1_700_000_000,
            "exp": 2_000_000_000_i64,
            "jti": "jti_test",
            "refresh_after": 1_701_296_000_i64,
        });
        let claims: LicenseClaims = serde_json::from_value(with_refresh).expect("parse");
        assert_eq!(claims.refresh_after, Some(1_701_296_000));

        let without_refresh = serde_json::json!({
            "iss": "https://api.fallow.cloud",
            "sub": "org_test",
            "tid": "tenant_test",
            "seats": 5,
            "tier": "pro",
            "features": ["runtime_coverage"],
            "iat": 1_700_000_000,
            "exp": 2_000_000_000_i64,
            "jti": "jti_test",
        });
        let claims: LicenseClaims = serde_json::from_value(without_refresh).expect("parse");
        assert_eq!(claims.refresh_after, None);
    }

    #[test]
    fn user_home_from_env_prefers_home_over_userprofile() {
        let getenv = |key: &str| match key {
            "HOME" => Some("/home/alice".to_owned()),
            "USERPROFILE" => Some(r"C:\Users\alice".to_owned()),
            _ => None,
        };
        assert_eq!(
            user_home_from_env(getenv),
            Some(PathBuf::from("/home/alice"))
        );
    }

    #[test]
    fn user_home_from_env_falls_back_to_userprofile_on_windows() {
        let getenv = |key: &str| match key {
            "USERPROFILE" => Some(r"C:\Users\alice".to_owned()),
            _ => None,
        };
        assert_eq!(
            user_home_from_env(getenv),
            Some(PathBuf::from(r"C:\Users\alice"))
        );
    }

    #[test]
    fn user_home_from_env_skips_empty_values() {
        let getenv = |key: &str| match key {
            "HOME" => Some(String::new()),
            "USERPROFILE" => Some(r"C:\Users\alice".to_owned()),
            _ => None,
        };
        assert_eq!(
            user_home_from_env(getenv),
            Some(PathBuf::from(r"C:\Users\alice"))
        );
    }

    #[test]
    fn user_home_from_env_returns_none_when_nothing_set() {
        assert_eq!(user_home_from_env(|_| None), None);
    }

    #[test]
    fn resolve_license_path_env_returns_none_for_unset() {
        assert_eq!(resolve_license_path_env(None), None);
    }

    #[test]
    fn resolve_license_path_env_returns_none_for_empty_string() {
        assert_eq!(resolve_license_path_env(Some(String::new())), None);
    }

    #[test]
    fn resolve_license_path_env_returns_none_for_whitespace_only() {
        assert_eq!(resolve_license_path_env(Some("   ".to_owned())), None);
        assert_eq!(resolve_license_path_env(Some("\t\n".to_owned())), None);
    }

    #[test]
    fn resolve_license_path_env_trims_surrounding_whitespace() {
        assert_eq!(
            resolve_license_path_env(Some("  /tmp/license.jwt  ".to_owned())),
            Some(PathBuf::from("/tmp/license.jwt"))
        );
    }

    #[test]
    fn resolve_license_path_env_returns_path_for_valid_value() {
        assert_eq!(
            resolve_license_path_env(Some("/etc/fallow/license.jwt".to_owned())),
            Some(PathBuf::from("/etc/fallow/license.jwt"))
        );
    }

    fn make_claims_with_iat(iat: i64, exp: i64) -> LicenseClaims {
        LicenseClaims {
            iss: "https://api.fallow.cloud".into(),
            sub: "org_test".into(),
            tid: "tenant_test".into(),
            seats: 5,
            tier: "pro".into(),
            features: vec!["runtime_coverage".into()],
            iat,
            exp,
            jti: "jti_test".into(),
            refresh_after: None,
        }
    }

    #[test]
    fn iat_within_tolerance_passes() {
        let (signing, verifying) = fixed_keypair();
        let now = 1_900_000_000;
        let claims = make_claims_with_iat(now + 3_600, now + 100 * SECONDS_PER_DAY);
        let jwt = sign_jwt(&signing, &claims);
        let status = verify_jwt_with_skew(
            &jwt,
            &verifying,
            now,
            DEFAULT_HARD_FAIL_DAYS,
            DEFAULT_SKEW_TOLERANCE_SECONDS,
        )
        .expect("within-tolerance JWT must verify");
        assert!(matches!(status, LicenseStatus::Valid { .. }));
    }

    #[test]
    fn iat_far_in_future_rejected_as_clock_skew() {
        let (signing, verifying) = fixed_keypair();
        let now = 1_900_000_000;
        let claims = make_claims_with_iat(now + 48 * 3_600, now + 100 * SECONDS_PER_DAY);
        let jwt = sign_jwt(&signing, &claims);
        let err = verify_jwt_with_skew(
            &jwt,
            &verifying,
            now,
            DEFAULT_HARD_FAIL_DAYS,
            DEFAULT_SKEW_TOLERANCE_SECONDS,
        )
        .expect_err("future-iat JWT must be rejected");
        assert!(
            matches!(err, LicenseError::ClockSkew { .. }),
            "expected ClockSkew, got {err:?}"
        );
    }

    #[test]
    fn clock_far_behind_iat_rejected_as_clock_skew() {
        let (signing, verifying) = fixed_keypair();
        let iat = 1_700_000_000;
        let now = iat - 60 * SECONDS_PER_DAY;
        let claims = make_claims_with_iat(iat, iat + 100 * SECONDS_PER_DAY);
        let jwt = sign_jwt(&signing, &claims);
        let err = verify_jwt_with_skew(
            &jwt,
            &verifying,
            now,
            DEFAULT_HARD_FAIL_DAYS,
            DEFAULT_SKEW_TOLERANCE_SECONDS,
        )
        .expect_err("clock-behind verification must be rejected");
        assert!(
            matches!(err, LicenseError::ClockSkew { .. }),
            "expected ClockSkew, got {err:?}"
        );
    }

    #[test]
    fn verify_jwt_shim_uses_default_tolerance() {
        let (signing, verifying) = fixed_keypair();
        let now = 1_900_000_000;
        let claims = make_claims_with_iat(now + 48 * 3_600, now + 100 * SECONDS_PER_DAY);
        let jwt = sign_jwt(&signing, &claims);
        let err = verify_jwt(&jwt, &verifying, now, DEFAULT_HARD_FAIL_DAYS)
            .expect_err("shim must reject 48h-future iat under default tolerance");
        assert!(matches!(err, LicenseError::ClockSkew { .. }));
    }

    #[test]
    fn clock_skew_display_is_human_friendly() {
        let err = LicenseError::ClockSkew {
            iat_seconds: 1_900_000_000 + 2 * SECONDS_PER_DAY,
            now_seconds: 1_900_000_000,
            tolerance_seconds: DEFAULT_SKEW_TOLERANCE_SECONDS,
        };
        let rendered = format!("{err}");
        assert!(
            !rendered.contains("iat"),
            "ClockSkew Display must not leak 'iat' jargon: {rendered}"
        );
        assert!(
            rendered.contains("days"),
            "ClockSkew Display must render a human-friendly duration: {rendered}"
        );
        assert!(
            rendered.contains("CI") || rendered.contains("NTP") || rendered.contains("drift"),
            "ClockSkew Display must name a non-user-error cause: {rendered}"
        );
        assert!(
            rendered.contains(SKEW_TOLERANCE_ENV),
            "ClockSkew Display must mention the env var override: {rendered}"
        );
    }

    #[test]
    fn skew_tolerance_seconds_from_env_parses_or_defaults() {
        let unset = |_: &str| None;
        assert_eq!(
            skew_tolerance_seconds_from(unset),
            DEFAULT_SKEW_TOLERANCE_SECONDS
        );

        let empty = |_: &str| Some(String::new());
        assert_eq!(
            skew_tolerance_seconds_from(empty),
            DEFAULT_SKEW_TOLERANCE_SECONDS
        );

        let whitespace = |_: &str| Some("   \t\n".to_owned());
        assert_eq!(
            skew_tolerance_seconds_from(whitespace),
            DEFAULT_SKEW_TOLERANCE_SECONDS
        );

        let garbage = |_: &str| Some("twenty".to_owned());
        assert_eq!(
            skew_tolerance_seconds_from(garbage),
            DEFAULT_SKEW_TOLERANCE_SECONDS
        );

        let negative = |_: &str| Some("-1".to_owned());
        assert_eq!(
            skew_tolerance_seconds_from(negative),
            DEFAULT_SKEW_TOLERANCE_SECONDS
        );

        let valid = |_: &str| Some("172800".to_owned());
        assert_eq!(skew_tolerance_seconds_from(valid), 172_800);

        let valid_trimmed = |_: &str| Some("  3600  ".to_owned());
        assert_eq!(skew_tolerance_seconds_from(valid_trimmed), 3_600);

        let huge = |_: &str| Some(u64::MAX.to_string());
        assert_eq!(skew_tolerance_seconds_from(huge), i64::MAX);
    }

    #[test]
    fn format_duration_seconds_renders_human_friendly() {
        assert_eq!(format_duration_seconds(0), "0 seconds");
        assert_eq!(format_duration_seconds(1), "1 second");
        assert_eq!(format_duration_seconds(45), "45 seconds");
        assert_eq!(format_duration_seconds(59), "59 seconds");
        assert_eq!(format_duration_seconds(60), "1 minute");
        assert_eq!(format_duration_seconds(90), "1 minute");
        assert_eq!(format_duration_seconds(120), "2 minutes");
        assert_eq!(format_duration_seconds(3_599), "59 minutes");
        assert_eq!(format_duration_seconds(3_600), "1 hour");
        assert_eq!(format_duration_seconds(3_660), "1 hour 1 minute");
        assert_eq!(format_duration_seconds(7_320), "2 hours 2 minutes");
        assert_eq!(format_duration_seconds(86_400), "1 day");
        assert_eq!(format_duration_seconds(90_000), "1 day 1 hour");
        assert_eq!(format_duration_seconds(172_800), "2 days");
        assert_eq!(format_duration_seconds(180_000), "2 days 2 hours");
    }
}
