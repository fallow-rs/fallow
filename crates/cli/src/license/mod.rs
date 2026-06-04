//! `fallow license` subcommand: activate, status, refresh, deactivate.
//!
//! All entry points are dispatched from [`run`]. Network-bound flows
//! (`refresh`, `activate --trial`) fetch a JWT from `api.fallow.cloud` and
//! then pass it through the same offline verifier used by the local activation
//! path. Local flows (`activate <jwt>`, `status`, `deactivate`) are fully
//! wired against [`fallow_license`].
//!
//! # Public key
//!
//! The Ed25519 verification key is compiled in at [`PUBLIC_KEY_BYTES`].

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ed25519_dalek::VerifyingKey;
use fallow_config::OutputFormat;
use fallow_license::{
    DEFAULT_HARD_FAIL_DAYS, Feature, LicenseClaims, LicenseError, LicenseStatus,
    current_unix_seconds, default_license_path, normalize_jwt, skew_tolerance_seconds_from_env,
    verify_jwt_with_skew,
};
use serde::{Deserialize, Serialize};

use crate::api::{
    NETWORK_EXIT_CODE, api_url, http_status_message, sanitize_network_error, try_api_agent,
};

/// Ed25519 verification key for fallow license JWT validation.
#[cfg(not(feature = "test-sidecar-key"))]
pub const PUBLIC_KEY_BYTES: [u8; 32] = [
    179, 203, 218, 13, 98, 63, 103, 172, 91, 108, 23, 122, 27, 101, 200, 182, 174, 117, 160, 41,
    167, 151, 66, 171, 13, 61, 148, 65, 181, 144, 24, 120,
];

/// Test-only license JWT verification key, derived from the deterministic seed
/// `[0xBB; 32]`. Enabled only by the `test-sidecar-key` cargo feature. The
/// `compile_error!` at `crates/cli/src/health/coverage.rs` enforces that this
/// feature stays out of release builds.
#[cfg(feature = "test-sidecar-key")]
pub const PUBLIC_KEY_BYTES: [u8; 32] = [
    0x7d, 0x59, 0xc5, 0x62, 0x3d, 0xd4, 0x0a, 0x74, 0xaa, 0x4d, 0x5a, 0x32, 0xac, 0x64, 0x5d, 0x3b,
    0x3f, 0x95, 0xda, 0xea, 0xe4, 0xc2, 0x2b, 0xe2, 0x54, 0x76, 0xdd, 0x6a, 0x48, 0x6f, 0x73, 0x82,
];
/// Subcommands for `fallow license`.
#[derive(Debug)]
pub enum LicenseSubcommand {
    /// Install a license JWT into `~/.fallow/license.jwt`.
    Activate(ActivateArgs),
    /// Print active license tier, seats, features, days remaining.
    Status,
    /// Fetch a fresh JWT from `api.fallow.cloud`, verify it offline, and
    /// persist it to the active license path.
    Refresh,
    /// Remove the local license file.
    Deactivate,
}

/// Arguments for `fallow license activate`.
#[derive(Clone, Default)]
pub struct ActivateArgs {
    /// JWT passed directly as a positional argument.
    pub raw_jwt: Option<String>,
    /// Path to a file containing the JWT.
    pub from_file: Option<PathBuf>,
    /// Read JWT from stdin.
    pub from_stdin: bool,
    /// Issue a 30-day email-gated trial via `api.fallow.cloud` and persist
    /// the returned JWT in one step.
    pub trial: bool,
    /// Email used for the trial flow (required when `trial = true`).
    pub email: Option<String>,
}

// Manual `Debug` masks the raw license JWT in logs.
impl std::fmt::Debug for ActivateArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ActivateArgs")
            .field("raw_jwt", &self.raw_jwt.as_ref().map(|_| "***"))
            .field("from_file", &self.from_file)
            .field("from_stdin", &self.from_stdin)
            .field("trial", &self.trial)
            .field("email", &self.email)
            .finish()
    }
}

#[derive(serde::Serialize)]
struct TrialRequest<'a> {
    email: &'a str,
}

#[derive(Deserialize)]
struct JwtResponse {
    jwt: String,
    /// Optional ISO-8601 trial expiry timestamp returned by the backend for
    /// `/v1/auth/license/trial`. Present only on the trial flow; `refresh`
    /// responses omit it. We surface it to the user on trial activation so
    /// they do not need to parse the JWT to see the trial end date.
    #[serde(default, rename = "trialEndsAt")]
    trial_ends_at: Option<String>,
}

/// The `kind` discriminator on the JSON envelope each subcommand emits under
/// `--format json`. Lets a consumer (e.g. the VS Code extension) tell a status
/// probe apart from a post-activation / post-refresh result.
#[derive(Clone, Copy)]
enum LicenseKind {
    Status,
    Activate,
    Refresh,
}

impl LicenseKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Status => "license-status",
            Self::Activate => "license-activate",
            Self::Refresh => "license-refresh",
        }
    }
}

/// Dispatch a `fallow license <sub>` invocation.
///
/// `output` is the globally-resolved [`OutputFormat`]; the human path is
/// byte-for-byte unchanged from before, and `--format json` switches every
/// subcommand to the machine-readable [`LicenseStatusJson`] / error envelope.
pub fn run(subcommand: &LicenseSubcommand, output: OutputFormat) -> ExitCode {
    let json = matches!(output, OutputFormat::Json);
    match subcommand {
        LicenseSubcommand::Activate(args) => run_activate(args, json),
        LicenseSubcommand::Status => run_status(json),
        LicenseSubcommand::Refresh => run_refresh(json),
        LicenseSubcommand::Deactivate => run_deactivate(json),
    }
}

/// Surface a setup / input error: JSON envelope on stdout under `--format json`
/// (so the extension parses one stream), plain stderr otherwise.
fn fail(message: &str, exit_code: u8, json: bool) -> ExitCode {
    if json {
        emit_error_json(message, exit_code);
    } else {
        eprintln!("fallow license: {message}");
    }
    ExitCode::from(exit_code)
}

fn emit_error_json(message: &str, exit_code: u8) {
    let error_obj = serde_json::json!({
        "error": true,
        "message": message,
        "exit_code": exit_code,
    });
    if let Ok(json) = serde_json::to_string_pretty(&error_obj) {
        println!("{json}");
    }
}

/// Render a verified status either as the existing human lines or as the JSON
/// envelope, depending on `json`. Used by every success path so the two
/// renderings never drift.
fn emit_status(status: &LicenseStatus, kind: LicenseKind, json: bool) {
    if json {
        print_status_json(status, kind);
    } else {
        print_status(status);
    }
}

fn run_activate(args: &ActivateArgs, json: bool) -> ExitCode {
    if args.trial {
        return run_trial(args.email.as_deref(), json);
    }
    let jwt = match read_jwt(args) {
        Ok(jwt) => jwt,
        Err(msg) => return fail(&msg, 2, json),
    };
    let key = match verifying_key() {
        Ok(k) => k,
        Err(msg) => return fail(&msg, 2, json),
    };
    match verify_jwt_with_skew(
        &jwt,
        &key,
        current_unix_seconds(),
        DEFAULT_HARD_FAIL_DAYS,
        skew_tolerance_seconds_from_env(),
    ) {
        Ok(status) => {
            if let Err(msg) = persist_jwt(&jwt, json) {
                return fail(&msg, 2, json);
            }
            emit_status(&status, LicenseKind::Activate, json);
            ExitCode::SUCCESS
        }
        Err(LicenseError::Truncated { .. }) => fail(
            &format!("{}", LicenseError::Truncated { actual: jwt.len() }),
            3,
            json,
        ),
        Err(err) => fail(&format!("failed to verify JWT: {err}"), 3, json),
    }
}

fn run_status(json: bool) -> ExitCode {
    let key = match verifying_key() {
        Ok(k) => k,
        Err(msg) => return fail(&msg, 2, json),
    };
    match fallow_license::load_and_verify(&key, DEFAULT_HARD_FAIL_DAYS) {
        Ok(status) => {
            emit_status(&status, LicenseKind::Status, json);
            match status {
                LicenseStatus::HardFail { .. } | LicenseStatus::Missing => ExitCode::from(3),
                _ => ExitCode::SUCCESS,
            }
        }
        Err(err) => fail(&format!("{err}"), 3, json),
    }
}

fn run_refresh(json: bool) -> ExitCode {
    match refresh_active_license(json) {
        Ok(status) => {
            emit_status(&status, LicenseKind::Refresh, json);
            ExitCode::SUCCESS
        }
        Err(message) => fail(&message, NETWORK_EXIT_CODE, json),
    }
}

fn run_trial(email: Option<&str>, json: bool) -> ExitCode {
    let Some(email) = email else {
        return fail("activate --trial requires --email <addr>", 2, json);
    };
    match activate_trial(email, json) {
        Ok(status) => {
            emit_status(&status, LicenseKind::Activate, json);
            ExitCode::SUCCESS
        }
        Err(message) => fail(&message, NETWORK_EXIT_CODE, json),
    }
}

fn run_deactivate(json: bool) -> ExitCode {
    let path = default_license_path();
    if !path.exists() {
        if json {
            print_deactivate_json(&path, false);
        } else {
            println!("fallow license: no license file at {}", path.display());
        }
        return ExitCode::SUCCESS;
    }
    match std::fs::remove_file(&path) {
        Ok(()) => {
            if json {
                print_deactivate_json(&path, true);
            } else {
                println!("fallow license: removed {}", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(err) => fail(
            &format!("failed to remove {}: {err}", path.display()),
            2,
            json,
        ),
    }
}

fn read_jwt(args: &ActivateArgs) -> Result<String, String> {
    if let Some(jwt) = args.raw_jwt.as_deref() {
        return Ok(normalize_jwt(jwt));
    }
    if let Some(path) = args.from_file.as_deref() {
        let raw = std::fs::read_to_string(path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        return Ok(normalize_jwt(&raw));
    }
    if args.from_stdin {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|err| format!("failed to read stdin: {err}"))?;
        return Ok(normalize_jwt(&buf));
    }
    Err(
        "no JWT provided. Pass it as a positional argument, --from-file <path>, or pipe via stdin (`-`).".to_owned(),
    )
}

fn persist_jwt(jwt: &str, json: bool) -> Result<(), String> {
    let path = write_jwt(jwt)?;
    if !json {
        println!("fallow license: stored at {}", path.display());
    }
    Ok(())
}

fn write_jwt(jwt: &str) -> Result<PathBuf, String> {
    let path = default_license_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    std::fs::write(&path, jwt)
        .map_err(|err| format!("failed to write {}: {err}", path.display()))?;
    restrict_license_permissions(&path)?;
    Ok(path)
}

/// Restrict the license file to owner-only read/write on Unix platforms.
///
/// The JWT is a bearer token; anyone who can read the file can use the
/// license. Home directories are typically 0700/0750 already, but setting
/// 0600 on the file itself is defense-in-depth for shared environments. No-op
/// on Windows (NTFS ACLs follow the parent directory).
#[cfg(unix)]
fn restrict_license_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)
        .map_err(|err| format!("failed to set permissions on {}: {err}", path.display()))
}

#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "mirrors Unix variant's Result signature for API consistency"
)]
fn restrict_license_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

/// Construct the compiled-in Ed25519 verification key.
///
/// Crate-internal so other CLI subcommands (e.g. `fallow coverage setup`)
/// can also detect license state without re-implementing key construction.
pub fn verifying_key() -> Result<VerifyingKey, String> {
    VerifyingKey::from_bytes(&PUBLIC_KEY_BYTES)
        .map_err(|err| format!("invalid compiled-in public key: {err}"))
}

pub fn activate_trial(email: &str, json: bool) -> Result<LicenseStatus, String> {
    let mut response = try_api_agent()
        .map_err(|err| err.to_string())?
        .post(&api_url("/v1/auth/license/trial"))
        .send_json(TrialRequest { email })
        .map_err(|err| sanitize_network_error(&format!("failed to request a trial: {err}")))?;
    if !response.status().is_success() {
        return Err(http_status_message(&mut response, "trial"));
    }
    store_verified_jwt(&mut response, "trial", json)
}

pub fn refresh_active_license(json: bool) -> Result<LicenseStatus, String> {
    let current = load_current_jwt()?;
    let mut response = try_api_agent()
        .map_err(|err| err.to_string())?
        .post(&api_url("/v1/auth/license/refresh"))
        .header("Authorization", &format!("Bearer {current}"))
        .send_empty()
        .map_err(|err| {
            sanitize_network_error(&format!("failed to refresh the current license: {err}"))
        })?;
    if !response.status().is_success() {
        return Err(http_status_message(&mut response, "refresh"));
    }
    store_verified_jwt(&mut response, "refresh", json)
}

fn load_current_jwt() -> Result<String, String> {
    match fallow_license::load_raw_jwt() {
        Ok(Some(jwt)) => Ok(jwt),
        Ok(None) => Err(
            "no license found. Run: fallow license activate --trial --email you@company.com"
                .to_owned(),
        ),
        Err(err) => Err(format!("failed to read the current license: {err}")),
    }
}

fn store_verified_jwt(
    response: &mut impl crate::api::ResponseBodyReader,
    operation: &str,
    json: bool,
) -> Result<LicenseStatus, String> {
    let payload: JwtResponse = response
        .read_json()
        .map_err(|err| format!("failed to parse {operation} response: {err}"))?;

    let jwt = normalize_jwt(&payload.jwt);
    let status = verify_downloaded_jwt(&jwt)?;
    let path = write_jwt(&jwt)?;
    if !json {
        println!("fallow license: stored at {}", path.display());
        if let Some(trial_ends_at) = payload.trial_ends_at.as_deref() {
            let trimmed = trial_ends_at.trim();
            if !trimmed.is_empty() {
                println!("fallow license: trial ends at {trimmed}");
            }
        }
    }
    Ok(status)
}

fn verify_downloaded_jwt(jwt: &str) -> Result<LicenseStatus, String> {
    let key = verifying_key()?;
    match verify_jwt_with_skew(
        jwt,
        &key,
        current_unix_seconds(),
        DEFAULT_HARD_FAIL_DAYS,
        skew_tolerance_seconds_from_env(),
    ) {
        Ok(status) => Ok(status),
        Err(LicenseError::Truncated { .. }) => {
            Err(format!("{}", LicenseError::Truncated { actual: jwt.len() }))
        }
        Err(err) => Err(format!("failed to verify JWT: {err}")),
    }
}

fn print_status(status: &LicenseStatus) {
    match status {
        LicenseStatus::Valid {
            claims,
            days_until_expiry,
        } => {
            println!(
                "license: VALID, tier={} seats={} features={} days_until_expiry={}",
                claims.tier,
                claims.seats,
                claims.features.join(","),
                days_until_expiry
            );
            if let Some(refresh_after) = claims.refresh_after
                && current_unix_seconds() >= refresh_after
            {
                println!(
                    "  refresh suggested now: fallow license refresh (prevents CI breakage before expiry)"
                );
            }
        }
        LicenseStatus::ExpiredWarning {
            claims,
            days_since_expiry,
        } => {
            println!(
                "license: EXPIRED ({days_since_expiry} days ago), analysis still runs in the warning window. \
                 Refresh: fallow license refresh"
            );
            println!(
                "  tier={} seats={} features={}",
                claims.tier,
                claims.seats,
                claims.features.join(",")
            );
        }
        LicenseStatus::ExpiredWatermark {
            claims,
            days_since_expiry,
        } => {
            println!(
                "license: EXPIRED ({days_since_expiry} days ago), output will show a watermark until refreshed. \
                 Refresh: fallow license refresh"
            );
            println!(
                "  tier={} seats={} features={}",
                claims.tier,
                claims.seats,
                claims.features.join(",")
            );
        }
        LicenseStatus::HardFail {
            days_since_expiry, ..
        } => {
            println!(
                "license: EXPIRED ({days_since_expiry} days ago, past grace window), paid features blocked. \
                 Refresh: fallow license refresh, or fallow license activate --trial --email <addr>"
            );
        }
        LicenseStatus::Missing => {
            println!(
                "license: NOT FOUND. Start a 30-day trial: fallow license activate --trial --email you@company.com"
            );
        }
    }
    if status.permits(&Feature::RuntimeCoverage) {
        println!("  → runtime_coverage: ENABLED");
    } else {
        println!("  → runtime_coverage: disabled (upgrade or refresh)");
    }
}

/// Machine-readable license status emitted under `--format json`.
///
/// The shape mirrors what [`print_status`] renders for humans, but as a stable
/// contract: `state` is the discriminant a UI keys on, `message` carries the
/// single human-facing sentence (so consumers never re-derive wording), and the
/// claim-derived fields (`tier` / `seats` / `features`) are `null` only on the
/// states that have no claims (`hard_fail` / `missing`). The raw JWT is NEVER
/// serialized; only the verified, derived claims.
#[derive(Serialize)]
struct LicenseStatusJson {
    kind: &'static str,
    schema_version: u32,
    /// One of `valid`, `expired_warning`, `expired_watermark`, `hard_fail`,
    /// `missing`.
    state: &'static str,
    tier: Option<String>,
    seats: Option<u32>,
    features: Vec<String>,
    days_until_expiry: Option<i64>,
    days_since_expiry: Option<u64>,
    refresh_suggested: bool,
    runtime_coverage_enabled: bool,
    license_path: String,
    message: String,
}

/// Schema version for [`LicenseStatusJson`]. Bump on any breaking field change.
const LICENSE_STATUS_SCHEMA_VERSION: u32 = 1;

/// True when the license is in `Valid` state and its `refresh_after` claim has
/// already passed (the human path prints a proactive-refresh hint here).
fn refresh_suggested(status: &LicenseStatus) -> bool {
    match status {
        LicenseStatus::Valid { claims, .. } => claims
            .refresh_after
            .is_some_and(|after| current_unix_seconds() >= after),
        _ => false,
    }
}

/// Borrow the `LicenseClaims` for the states that carry them.
fn status_claims(status: &LicenseStatus) -> Option<&LicenseClaims> {
    match status {
        LicenseStatus::Valid { claims, .. }
        | LicenseStatus::ExpiredWarning { claims, .. }
        | LicenseStatus::ExpiredWatermark { claims, .. }
        | LicenseStatus::HardFail { claims, .. } => Some(claims),
        LicenseStatus::Missing => None,
    }
}

/// The machine discriminant for a status. Kept in lockstep with the union the
/// VS Code extension hand-types in `license-types.ts`.
fn status_state(status: &LicenseStatus) -> &'static str {
    match status {
        LicenseStatus::Valid { .. } => "valid",
        LicenseStatus::ExpiredWarning { .. } => "expired_warning",
        LicenseStatus::ExpiredWatermark { .. } => "expired_watermark",
        LicenseStatus::HardFail { .. } => "hard_fail",
        LicenseStatus::Missing => "missing",
    }
}

/// Compose the single human-facing sentence for a status. Distinct from
/// [`print_status`]'s multi-line, hint-laden output: this is the one line a UI
/// shows verbatim (status bar tooltip, info toast), so it states the fact
/// plainly without embedding CLI remediation commands.
fn status_message(status: &LicenseStatus) -> String {
    match status {
        LicenseStatus::Valid {
            claims,
            days_until_expiry,
        } => format!(
            "License active ({}, {} seat{}), {} day{} until expiry.",
            claims.tier,
            claims.seats,
            plural(u64::from(claims.seats)),
            days_until_expiry,
            plural(days_until_expiry.unsigned_abs()),
        ),
        LicenseStatus::ExpiredWarning {
            days_since_expiry, ..
        } => format!(
            "License expired {} day{} ago. Analysis still runs; refresh to clear the warning.",
            days_since_expiry,
            plural(*days_since_expiry),
        ),
        LicenseStatus::ExpiredWatermark {
            days_since_expiry, ..
        } => format!(
            "License expired {} day{} ago. Output is watermarked until you refresh.",
            days_since_expiry,
            plural(*days_since_expiry),
        ),
        LicenseStatus::HardFail {
            days_since_expiry, ..
        } => format!(
            "License expired {} day{} ago, past the grace window. Paid features are blocked until you refresh or start a trial.",
            days_since_expiry,
            plural(*days_since_expiry),
        ),
        LicenseStatus::Missing => {
            "No license active. Start a 30-day trial or activate a license token.".to_owned()
        }
    }
}

const fn plural(n: u64) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Resolve the license path the loader would actually use, honoring the
/// `$FALLOW_LICENSE_PATH` override and falling back to the canonical default.
/// `$FALLOW_LICENSE` (inline JWT, no file) is reported as the default path,
/// since there is no file to point at.
fn active_license_path() -> String {
    if let Ok(raw) = std::env::var("FALLOW_LICENSE_PATH") {
        let trimmed = raw.trim();
        if !trimmed.is_empty() {
            return trimmed.to_owned();
        }
    }
    default_license_path().display().to_string()
}

fn print_status_json(status: &LicenseStatus, kind: LicenseKind) {
    let claims = status_claims(status);
    let (days_until_expiry, days_since_expiry) = match status {
        LicenseStatus::Valid {
            days_until_expiry, ..
        } => (Some(*days_until_expiry), None),
        LicenseStatus::ExpiredWarning {
            days_since_expiry, ..
        }
        | LicenseStatus::ExpiredWatermark {
            days_since_expiry, ..
        }
        | LicenseStatus::HardFail {
            days_since_expiry, ..
        } => (None, Some(*days_since_expiry)),
        LicenseStatus::Missing => (None, None),
    };

    let payload = LicenseStatusJson {
        kind: kind.as_str(),
        schema_version: LICENSE_STATUS_SCHEMA_VERSION,
        state: status_state(status),
        tier: claims.map(|c| c.tier.clone()),
        seats: claims.map(|c| c.seats),
        features: claims.map(|c| c.features.clone()).unwrap_or_default(),
        days_until_expiry,
        days_since_expiry,
        refresh_suggested: refresh_suggested(status),
        runtime_coverage_enabled: status.permits(&Feature::RuntimeCoverage),
        license_path: active_license_path(),
        message: status_message(status),
    };

    if let Ok(json) = serde_json::to_string_pretty(&payload) {
        println!("{json}");
    }
}

/// JSON envelope for `fallow license deactivate --format json`.
fn print_deactivate_json(path: &Path, removed: bool) {
    let message = if removed {
        format!("License removed from {}.", path.display())
    } else {
        format!("No license file at {} to remove.", path.display())
    };
    let payload = serde_json::json!({
        "kind": "license-deactivate",
        "schema_version": LICENSE_STATUS_SCHEMA_VERSION,
        "state": "missing",
        "removed": removed,
        "license_path": path.display().to_string(),
        "message": message,
    });
    if let Ok(json) = serde_json::to_string_pretty(&payload) {
        println!("{json}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activate_args_debug_masks_raw_jwt() {
        let args = ActivateArgs {
            raw_jwt: Some("eyJhbGciOiJFZERTQSJ9.secret_payload.sig".to_owned()),
            email: Some("alice@example.com".to_owned()),
            ..ActivateArgs::default()
        };
        let formatted = format!("{args:?}");
        assert!(
            !formatted.contains("secret_payload"),
            "raw_jwt leaked through Debug: {formatted}"
        );
        assert!(
            formatted.contains("raw_jwt: Some(\"***\")"),
            "expected explicit redaction marker, got: {formatted}"
        );
        let bare = ActivateArgs::default();
        assert!(format!("{bare:?}").contains("raw_jwt: None"));
        assert!(formatted.contains("email: Some(\"alice@example.com\")"));
    }

    #[test]
    fn read_jwt_prefers_raw_arg() {
        let args = ActivateArgs {
            raw_jwt: Some("a.b.c".into()),
            ..Default::default()
        };
        assert_eq!(read_jwt(&args).unwrap(), "a.b.c");
    }

    #[test]
    fn read_jwt_normalizes_whitespace() {
        let args = ActivateArgs {
            raw_jwt: Some("a  .b\nc".into()),
            ..Default::default()
        };
        assert_eq!(read_jwt(&args).unwrap(), "a.bc");
    }

    #[test]
    fn read_jwt_errors_when_no_source() {
        let args = ActivateArgs::default();
        assert!(read_jwt(&args).is_err());
    }

    #[test]
    fn run_trial_without_email_errors() {
        let exit = run_trial(None, false);
        assert_eq!(format!("{exit:?}"), format!("{:?}", ExitCode::from(2)));
    }

    fn sample_claims(features: &[&str]) -> LicenseClaims {
        LicenseClaims {
            iss: "https://api.fallow.cloud".to_owned(),
            sub: "org_1".to_owned(),
            tid: "tenant_1".to_owned(),
            seats: 5,
            tier: "team".to_owned(),
            features: features.iter().map(|s| (*s).to_owned()).collect(),
            iat: 1_700_000_000,
            exp: 1_800_000_000,
            jti: "jti_1".to_owned(),
            refresh_after: None,
        }
    }

    fn json_value(status: &LicenseStatus, kind: LicenseKind) -> serde_json::Value {
        let claims = status_claims(status);
        let (days_until_expiry, days_since_expiry) = match status {
            LicenseStatus::Valid {
                days_until_expiry, ..
            } => (Some(*days_until_expiry), None),
            LicenseStatus::ExpiredWarning {
                days_since_expiry, ..
            }
            | LicenseStatus::ExpiredWatermark {
                days_since_expiry, ..
            }
            | LicenseStatus::HardFail {
                days_since_expiry, ..
            } => (None, Some(*days_since_expiry)),
            LicenseStatus::Missing => (None, None),
        };
        let payload = LicenseStatusJson {
            kind: kind.as_str(),
            schema_version: LICENSE_STATUS_SCHEMA_VERSION,
            state: status_state(status),
            tier: claims.map(|c| c.tier.clone()),
            seats: claims.map(|c| c.seats),
            features: claims.map(|c| c.features.clone()).unwrap_or_default(),
            days_until_expiry,
            days_since_expiry,
            refresh_suggested: refresh_suggested(status),
            runtime_coverage_enabled: status.permits(&Feature::RuntimeCoverage),
            license_path: active_license_path(),
            message: status_message(status),
        };
        serde_json::to_value(&payload).unwrap()
    }

    #[test]
    fn status_json_valid_has_all_documented_keys() {
        let status = LicenseStatus::Valid {
            claims: sample_claims(&["runtime_coverage"]),
            days_until_expiry: 12,
        };
        let value = json_value(&status, LicenseKind::Status);
        let obj = value.as_object().unwrap();
        for key in [
            "kind",
            "schema_version",
            "state",
            "tier",
            "seats",
            "features",
            "days_until_expiry",
            "days_since_expiry",
            "refresh_suggested",
            "runtime_coverage_enabled",
            "license_path",
            "message",
        ] {
            assert!(obj.contains_key(key), "missing key: {key}");
        }
        assert_eq!(value["kind"], "license-status");
        assert_eq!(value["state"], "valid");
        assert_eq!(value["tier"], "team");
        assert_eq!(value["seats"], 5);
        assert_eq!(value["days_until_expiry"], 12);
        assert!(value["days_since_expiry"].is_null());
        assert_eq!(value["runtime_coverage_enabled"], true);
        assert_eq!(value["refresh_suggested"], false);
        assert!(
            value["message"]
                .as_str()
                .unwrap()
                .contains("License active")
        );
    }

    #[test]
    fn status_json_missing_nulls_claims() {
        let value = json_value(&LicenseStatus::Missing, LicenseKind::Status);
        assert_eq!(value["state"], "missing");
        assert!(value["tier"].is_null());
        assert!(value["seats"].is_null());
        assert_eq!(value["features"].as_array().unwrap().len(), 0);
        assert_eq!(value["runtime_coverage_enabled"], false);
        assert!(value["days_until_expiry"].is_null());
        assert!(value["days_since_expiry"].is_null());
    }

    #[test]
    fn status_json_hard_fail_reports_days_since_and_no_runtime_coverage() {
        let status = LicenseStatus::HardFail {
            claims: sample_claims(&["runtime_coverage"]),
            days_since_expiry: 45,
        };
        let value = json_value(&status, LicenseKind::Status);
        assert_eq!(value["state"], "hard_fail");
        assert_eq!(value["days_since_expiry"], 45);
        // HardFail blocks paid features even though the claim lists the feature.
        assert_eq!(value["runtime_coverage_enabled"], false);
    }

    #[test]
    fn status_json_expired_warning_keeps_claims_visible() {
        let status = LicenseStatus::ExpiredWarning {
            claims: sample_claims(&["runtime_coverage"]),
            days_since_expiry: 3,
        };
        let value = json_value(&status, LicenseKind::Status);
        assert_eq!(value["state"], "expired_warning");
        assert_eq!(value["tier"], "team");
        assert_eq!(value["days_since_expiry"], 3);
        // Warning window still permits the feature.
        assert_eq!(value["runtime_coverage_enabled"], true);
    }

    #[test]
    fn activate_and_refresh_kinds_are_distinct() {
        let status = LicenseStatus::Missing;
        assert_eq!(
            json_value(&status, LicenseKind::Activate)["kind"],
            "license-activate"
        );
        assert_eq!(
            json_value(&status, LicenseKind::Refresh)["kind"],
            "license-refresh"
        );
    }

    #[test]
    fn refresh_suggested_true_when_refresh_after_passed() {
        let mut claims = sample_claims(&["runtime_coverage"]);
        claims.refresh_after = Some(current_unix_seconds() - 10);
        let status = LicenseStatus::Valid {
            claims,
            days_until_expiry: 5,
        };
        let value = json_value(&status, LicenseKind::Status);
        assert_eq!(value["refresh_suggested"], true);
    }

    #[test]
    fn message_pluralizes_seats_and_days() {
        let mut claims = sample_claims(&[]);
        claims.seats = 1;
        let status = LicenseStatus::Valid {
            claims,
            days_until_expiry: 1,
        };
        let msg = status_message(&status);
        assert!(msg.contains("1 seat)"), "got: {msg}");
        assert!(msg.contains("1 day until expiry"), "got: {msg}");
    }
}
